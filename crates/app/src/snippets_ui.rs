//! The "Snippets" tab: folders, tags, a keyword and a hotkey per snippet, placeholders, and the pinned entries.

use clipvault_core::config::Lang;
use clipvault_core::hotkey::parse_hotkey;
use clipvault_core::i18n::S;
use clipvault_core::snippets::{Snippet, expand, parse_tags};
use clipvault_core::{Item, now_unix};
use eframe::egui::{self, Align, Layout, RichText, Sense, Ui, vec2};

use crate::{App, platform, theme, winclip};

/// The editor's fields while a snippet is being typed.
#[derive(Clone, Default)]
pub struct SnipDraft {
    pub id: u64,
    pub name: String,
    pub folder: String,
    pub tags: String,
    pub keyword: String,
    pub hotkey: String,
    pub text: String,
}

impl SnipDraft {
    pub fn from(s: &Snippet) -> SnipDraft {
        SnipDraft {
            id: s.id,
            name: s.name.clone(),
            folder: s.folder.clone(),
            tags: s.tags.join(", "),
            keyword: s.keyword.clone(),
            hotkey: s.hotkey.clone(),
            text: s.text.clone(),
        }
    }

    fn to_snippet(&self) -> Snippet {
        Snippet {
            id: self.id,
            name: self.name.trim().to_owned(),
            text: self.text.clone(),
            folder: self.folder.trim().to_owned(),
            tags: parse_tags(&self.tags),
            keyword: self.keyword.trim().to_owned(),
            hotkey: self.hotkey.trim().to_owned(),
            updated: 0,
        }
    }
}

impl App {
    fn paste_snippet_text(&mut self, template: &str) {
        let clip = winclip::read_text(std::ptr::null_mut()).unwrap_or_default();
        let text = expand(template, &winclip::local_clock(), &clip);
        self.request_paste_text(text);
    }

    pub fn paste_snippet(&mut self, id: u64) {
        let tpl = self
            .shared
            .history
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .snippet(id)
            .map(|s| s.text.clone());
        if let Some(t) = tpl {
            self.paste_snippet_text(&t);
        }
    }

    pub fn snippets_ui(&mut self, ui: &mut Ui) {
        let lang = self.shared.lang();
        egui::Panel::left("snip_list")
            .resizable(false)
            .exact_size(380.0)
            .show(ui, |ui| self.snippet_list(ui, lang));
        egui::CentralPanel::default().show(ui, |ui| self.snippet_editor(ui, lang));
    }

    fn snippet_list(&mut self, ui: &mut Ui, lang: Lang) {
        ui.add(
            egui::TextEdit::singleline(&mut self.popup.snip_query)
                .hint_text(S::SnipSearch.get(lang))
                .desired_width(f32::INFINITY),
        );
        if ui.button(S::SnipNew.get(lang)).clicked() {
            self.popup.snip_draft = SnipDraft::default();
            self.popup.snip_editing = true;
        }
        let (snips, pinned): (Vec<Snippet>, Vec<Item>) = {
            let h = self.shared.history.lock().unwrap_or_else(|e| e.into_inner());
            let snips = h
                .snippets()
                .into_iter()
                .filter(|s| s.matches(&self.popup.snip_query))
                .cloned()
                .collect();
            let pinned = h
                .query(&clipvault_core::Query {
                    pinned_only: true,
                    ..Default::default()
                })
                .into_iter()
                .cloned()
                .collect();
            (snips, pinned)
        };
        let mut action: Option<ListAction> = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            if snips.is_empty() && self.popup.snip_query.is_empty() {
                ui.add_space(10.0);
                ui.label(RichText::new(S::SnipEmpty.get(lang)).weak());
            }
            let mut folder: Option<&str> = None;
            for s in &snips {
                if folder != Some(s.folder.as_str()) {
                    folder = Some(s.folder.as_str());
                    ui.add_space(6.0);
                    let label = if s.folder.is_empty() { "—" } else { s.folder.as_str() };
                    ui.label(RichText::new(label).small().strong().color(theme::ACCENT));
                }
                let selected = self.popup.snip_draft.id == s.id && self.popup.snip_editing;
                let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 38.0), Sense::click());
                if selected {
                    ui.painter().rect_filled(rect, 6.0, theme::ROW_SELECTED);
                } else if resp.hovered() {
                    ui.painter().rect_filled(rect, 6.0, theme::ROW_HOVER);
                }
                let tags = if s.tags.is_empty() {
                    String::new()
                } else {
                    format!("  #{}", s.tags.join(" #"))
                };
                let kw = if s.keyword.is_empty() {
                    String::new()
                } else {
                    format!("  [{}]", s.keyword)
                };
                ui.painter().text(
                    rect.left_top() + vec2(10.0, 11.0),
                    egui::Align2::LEFT_CENTER,
                    &s.name,
                    egui::FontId::proportional(14.0),
                    egui::Color32::from_gray(225),
                );
                ui.painter().text(
                    rect.left_bottom() + vec2(10.0, -10.0),
                    egui::Align2::LEFT_CENTER,
                    format!("{}{kw}{tags}", one_line(&s.text, 40)),
                    egui::FontId::proportional(11.0),
                    egui::Color32::from_gray(140),
                );
                if resp.clicked() {
                    action = Some(ListAction::Edit(s.clone()));
                }
                if resp.double_clicked() {
                    action = Some(ListAction::Paste(s.text.clone()));
                }
            }
            if !pinned.is_empty() {
                ui.add_space(10.0);
                ui.label(
                    RichText::new(S::SnipPinned.get(lang))
                        .small()
                        .strong()
                        .color(theme::WARN),
                );
                for it in &pinned {
                    let label = if it.volatile {
                        "••••••••".to_owned()
                    } else {
                        one_line(&it.text, 60)
                    };
                    if ui
                        .add(
                            egui::Button::new(label)
                                .frame(false)
                                .min_size(vec2(ui.available_width(), 26.0)),
                        )
                        .clicked()
                    {
                        action = Some(ListAction::PasteEntry(it.id));
                    }
                }
            }
        });
        match action {
            Some(ListAction::Edit(s)) => {
                self.popup.snip_draft = SnipDraft::from(&s);
                self.popup.snip_editing = true;
            }
            Some(ListAction::Paste(t)) => self.paste_snippet_text(&t),
            Some(ListAction::PasteEntry(id)) => self.request_paste(id),
            None => {}
        }
    }

    fn snippet_editor(&mut self, ui: &mut Ui, lang: Lang) {
        if !self.popup.snip_editing {
            ui.add_space(60.0);
            ui.vertical_centered(|ui| ui.label(RichText::new(S::SelectEntry.get(lang)).weak()));
            return;
        }
        let mut save = false;
        let mut delete = false;
        let mut paste = false;
        {
            let d = &mut self.popup.snip_draft;
            egui::Grid::new("snip_form")
                .num_columns(2)
                .spacing([10.0, 6.0])
                .show(ui, |ui| {
                    ui.label(S::SnipName.get(lang));
                    ui.add(egui::TextEdit::singleline(&mut d.name).desired_width(f32::INFINITY));
                    ui.end_row();
                    ui.label(S::SnipFolder.get(lang));
                    ui.add(egui::TextEdit::singleline(&mut d.folder).desired_width(f32::INFINITY));
                    ui.end_row();
                    ui.label(S::SnipTags.get(lang));
                    ui.add(egui::TextEdit::singleline(&mut d.tags).desired_width(f32::INFINITY));
                    ui.end_row();
                    ui.label(S::SnipKeyword.get(lang));
                    ui.add(egui::TextEdit::singleline(&mut d.keyword).desired_width(160.0));
                    ui.end_row();
                    ui.label(S::SnipHotkey.get(lang));
                    ui.horizontal(|ui| {
                        crate::settings_ui::hotkey_field(
                            ui,
                            &mut self.popup.capturing,
                            "snip_hotkey",
                            &mut d.hotkey,
                            true,
                            lang,
                        );
                        if !d.hotkey.trim().is_empty() && parse_hotkey(&d.hotkey).is_none() {
                            ui.label(RichText::new(S::SnipHotkeyBad.get(lang)).color(theme::BAD));
                        }
                    });
                    ui.end_row();
                });
            ui.label(S::SnipText.get(lang));
            ui.add(
                egui::TextEdit::multiline(&mut d.text)
                    .desired_rows(7)
                    .desired_width(f32::INFINITY)
                    .font(egui::TextStyle::Monospace),
            );
        }
        let preview = {
            let d = &self.popup.snip_draft;
            expand(&d.text, &winclip::local_clock(), "…")
        };
        ui.label(RichText::new(S::SnipPreview.get(lang)).weak().small());
        theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add(egui::Label::new(RichText::new(&preview).monospace()).wrap());
        });
        ui.add_space(6.0);
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            let ok = !self.popup.snip_draft.name.trim().is_empty()
                && (self.popup.snip_draft.hotkey.trim().is_empty()
                    || parse_hotkey(&self.popup.snip_draft.hotkey).is_some());
            if ui
                .add_enabled(ok, egui::Button::new(RichText::new(S::SnipSave.get(lang)).strong()))
                .clicked()
            {
                save = true;
            }
            if theme::primary_button(ui, S::Paste.get(lang)).clicked() {
                paste = true;
            }
            if self.popup.snip_draft.id != 0
                && ui
                    .button(RichText::new(S::Delete.get(lang)).color(theme::BAD))
                    .clicked()
            {
                delete = true;
            }
        });
        if paste {
            let t = self.popup.snip_draft.text.clone();
            self.paste_snippet_text(&t);
        }
        if save {
            let s = self.popup.snip_draft.to_snippet();
            let r = self
                .shared
                .history
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .put_snippet(s, now_unix());
            if let Ok(id) = r {
                self.popup.snip_draft.id = id;
                platform::post(platform::WM_REHOTKEY);
            }
        }
        if delete {
            let id = self.popup.snip_draft.id;
            let _ = self
                .shared
                .history
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .delete_snippet(id);
            self.popup.snip_draft = SnipDraft::default();
            self.popup.snip_editing = false;
            platform::post(platform::WM_REHOTKEY);
        }
    }
}

enum ListAction {
    Edit(Snippet),
    Paste(String),
    PasteEntry(u64),
}

fn one_line(text: &str, max: usize) -> String {
    let t: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let cut: String = t.chars().take(max).collect();
    if t.chars().count() > max {
        format!("{cut}…")
    } else {
        cut
    }
}
