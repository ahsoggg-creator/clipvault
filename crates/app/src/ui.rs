//! The popup: search, filters, the list of entries and the preview.

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use clipvault_core::classify::parse_color;
use clipvault_core::clipfmt;
use clipvault_core::config::Lang;
use clipvault_core::i18n::{S, fmt};
use clipvault_core::transform::Transform;
use clipvault_core::{Item, Kind, Query, now_unix};
use eframe::egui::{
    self, Align, Align2, Color32, FontId, Key, Layout, Modifiers, RichText, Sense, TextureHandle, TextureOptions, Ui,
    vec2,
};

use crate::shared::PasteRequest;
use crate::theme;
use crate::{App, platform};

const ROW_H: f32 = 40.0;
pub(crate) const MARK: Color32 = Color32::from_rgb(190, 180, 255);
const DAY: u64 = 86_400;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DateFilter {
    Any,
    Today,
    Week,
    Month,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum View {
    History,
    Snippets,
    Settings,
}

#[derive(PartialEq, Clone)]
struct CacheKey {
    query: String,
    kind: Option<Kind>,
    app: Option<String>,
    date: DateFilter,
    pinned: bool,
    rev: u64,
    day: u64,
}

pub struct Popup {
    pub view: View,
    pub query: String,
    pub kind: Option<Kind>,
    pub app: Option<String>,
    pub date: DateFilter,
    pub pinned_only: bool,
    pub selected: Option<u64>,
    cache_key: Option<CacheKey>,
    pub results: Vec<u64>,
    pub seen_seq: u64,
    pub shown_at: Instant,
    pub was_focused: bool,
    pub confirm_clear: bool,
    pub tex: Option<(u64, TextureHandle)>,
    /// The hotkey field that is recording a combination right now.
    pub capturing: Option<egui::Id>,
    /// The title strip is being dragged.
    pub drag: Option<platform::Drag>,
    /// Entries picked with Ctrl+click, in the order they were picked: pasted together.
    pub marked: Vec<u64>,
    /// The Win+V setting changed and Explorer has not been restarted yet.
    pub win_v_hint: bool,
    pub scroll_to_selected: bool,
    pub focus_search: bool,
    pub toast: Option<(String, Instant)>,
    /// The memory-only secret whose text the user chose to show.
    pub revealed: Option<u64>,
    pub custom_text: String,
    pub blacklist_text: String,
    pub texts_loaded: bool,
    pub scroll_to_secrets: bool,
    pub scroll_to_hotkeys: bool,
    pub snip_query: String,
    pub snip_draft: crate::snippets_ui::SnipDraft,
    pub snip_editing: bool,
    /// The snippet whose keyword is typed in the search box right now.
    pub keyword_snippet: Option<(u64, String)>,
}

impl Default for Popup {
    fn default() -> Self {
        Popup {
            view: View::History,
            query: String::new(),
            kind: None,
            app: None,
            date: DateFilter::Any,
            pinned_only: false,
            selected: None,
            cache_key: None,
            results: Vec::new(),
            seen_seq: 0,
            shown_at: Instant::now(),
            was_focused: false,
            confirm_clear: false,
            tex: None,
            capturing: None,
            drag: None,
            marked: Vec::new(),
            win_v_hint: false,
            scroll_to_selected: false,
            focus_search: true,
            toast: None,
            revealed: None,
            custom_text: String::new(),
            blacklist_text: String::new(),
            texts_loaded: false,
            scroll_to_secrets: false,
            scroll_to_hotkeys: false,
            snip_query: String::new(),
            snip_draft: Default::default(),
            snip_editing: false,
            keyword_snippet: None,
        }
    }
}

impl Popup {
    pub fn cache_key_reset(&mut self) {
        self.cache_key = None;
    }

    pub fn reset_filters(&mut self) {
        self.query.clear();
        self.kind = None;
        self.app = None;
        self.date = DateFilter::Any;
        self.pinned_only = false;
        self.confirm_clear = false;
        self.capturing = None;
        self.drag = None;
        self.marked.clear();
        self.selected = None;
        self.revealed = None;
        self.texts_loaded = false;
        self.scroll_to_selected = true;
        self.focus_search = true;
    }
}

/// `5 мин назад` / `3 h ago` / a date for old entries.
pub fn ago(lang: Lang, now: u64, then: u64) -> String {
    let d = now.saturating_sub(then);
    let (s, n) = match d {
        0..60 => (S::AgoNow, 0),
        60..3600 => (S::AgoMin, d / 60),
        3600..86_400 => (S::AgoHour, d / 3600),
        86_400..2_592_000 => (S::AgoDay, d / 86_400),
        _ => {
            let (y, m, dd, ..) = clipvault_core::timefmt::civil(then);
            return format!("{dd:02}.{m:02}.{y}");
        }
    };
    fmt(s, lang, &[&n.to_string()])
}

pub fn human_size(bytes: u64) -> String {
    match bytes {
        0..1024 => format!("{bytes} B"),
        1024..1_048_576 => format!("{:.1} KB", bytes as f64 / 1024.0),
        _ => format!("{:.1} MB", bytes as f64 / 1_048_576.0),
    }
}

pub fn transform_label(t: Transform) -> S {
    match t {
        Transform::JsonFormat => S::TrJsonFormat,
        Transform::JsonMinify => S::TrJsonMinify,
        Transform::Base64Encode => S::TrBase64Enc,
        Transform::Base64Decode => S::TrBase64Dec,
        Transform::UrlEncode => S::TrUrlEnc,
        Transform::UrlDecode => S::TrUrlDec,
        Transform::LayoutEnToRu => S::TrLayoutEnRu,
        Transform::LayoutRuToEn => S::TrLayoutRuEn,
        Transform::LayoutAuto => S::TrLayoutAuto,
        Transform::ExtractLinks => S::TrLinks,
        Transform::ExtractEmails => S::TrEmails,
        Transform::Upper => S::TrUpper,
        Transform::Lower => S::TrLower,
        Transform::Title => S::TrTitle,
        Transform::Trim => S::TrTrim,
        Transform::SortLines => S::TrSort,
        Transform::SortLinesDesc => S::TrSortDesc,
        Transform::UniqueLines => S::TrUnique,
    }
}

pub(crate) fn one_line(text: &str, max: usize) -> String {
    let mut out = String::new();
    let mut last_space = true;
    for c in text.chars() {
        let c = if c.is_whitespace() { ' ' } else { c };
        if c == ' ' && last_space {
            continue;
        }
        last_space = c == ' ';
        out.push(c);
        if out.chars().count() >= max {
            break;
        }
    }
    out
}

impl App {
    fn day_start(now: u64) -> u64 {
        // local midnight would need the time zone; "today" = the last 24 hours keeps this dependency-free
        now.saturating_sub(DAY)
    }

    fn since(&self, now: u64) -> Option<u64> {
        match self.popup.date {
            DateFilter::Any => None,
            DateFilter::Today => Some(Self::day_start(now)),
            DateFilter::Week => Some(now.saturating_sub(7 * DAY)),
            DateFilter::Month => Some(now.saturating_sub(30 * DAY)),
        }
    }

    /// Re-runs the search when the query, a filter or the history changed.
    fn refresh_results(&mut self, now: u64) {
        let h = self.shared.history.lock().unwrap_or_else(|e| e.into_inner());
        let key = CacheKey {
            query: self.popup.query.clone(),
            kind: self.popup.kind,
            app: self.popup.app.clone(),
            date: self.popup.date,
            pinned: self.popup.pinned_only,
            rev: h.revision(),
            day: now / 60,
        };
        if self.popup.cache_key.as_ref() == Some(&key) {
            drop(h);
            self.ensure_selection();
            return;
        }
        let q = Query {
            text: self.popup.query.clone(),
            kinds: self.popup.kind.into_iter().collect(),
            app: self.popup.app.clone(),
            since: self.since(now),
            pinned_only: self.popup.pinned_only,
        };
        self.popup.results = h.query(&q).iter().map(|i| i.id).collect();
        drop(h);
        self.popup.cache_key = Some(key);
        self.ensure_selection();
    }

    /// The selection must always point at a visible entry (the first one by default).
    fn ensure_selection(&mut self) {
        let first = self.popup.results.first().copied();
        if self.popup.selected.is_none_or(|s| !self.popup.results.contains(&s)) {
            self.popup.selected = first;
        }
    }

    pub(crate) fn selected_index(&self) -> Option<usize> {
        let id = self.popup.selected?;
        self.popup.results.iter().position(|&r| r == id)
    }

    fn move_selection(&mut self, delta: isize) {
        if self.popup.results.is_empty() {
            return;
        }
        let cur = self.selected_index().unwrap_or(0) as isize;
        let next = (cur + delta).clamp(0, self.popup.results.len() as isize - 1) as usize;
        self.popup.selected = Some(self.popup.results[next]);
        self.popup.scroll_to_selected = true;
    }

    pub fn request_paste(&mut self, id: u64) {
        if self.shared.demo {
            self.popup.toast = Some(("Демо: вставка отключена".into(), Instant::now()));
            return;
        }
        *self.shared.paste.lock().unwrap_or_else(|e| e.into_inner()) = Some(PasteRequest {
            id,
            text: None,
            copy_only: false,
        });
        platform::post(platform::WM_PASTE);
    }

    /// Pastes this text (a transformed entry, an expanded snippet) into the window the popup was opened from.
    pub fn request_paste_text(&mut self, text: String) {
        if self.shared.demo {
            self.popup.toast = Some(("Демо: вставка отключена".into(), Instant::now()));
            return;
        }
        *self.shared.paste.lock().unwrap_or_else(|e| e.into_inner()) = Some(PasteRequest {
            id: 0,
            text: Some(text),
            copy_only: false,
        });
        platform::post(platform::WM_PASTE);
    }

    /// Puts the entry on the clipboard without pasting it anywhere.
    pub(crate) fn request_copy(&mut self, id: u64) {
        if self.shared.demo {
            self.popup.toast = Some(("Демо: копирование отключено".into(), Instant::now()));
            return;
        }
        *self.shared.paste.lock().unwrap_or_else(|e| e.into_inner()) = Some(PasteRequest {
            id,
            text: None,
            copy_only: true,
        });
        platform::post(platform::WM_PASTE);
    }

    /// Deletes the history (the pinned entries stay when `keep_pinned`) and empties the system clipboard too.
    pub(crate) fn clear_everything(&mut self, keep_pinned: bool) {
        let _ = self
            .shared
            .history
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear(keep_pinned);
        if !self.shared.demo {
            platform::post(platform::WM_CLEAR_CLIPBOARD);
        }
        self.popup.confirm_clear = false;
        self.popup.selected = None;
        self.popup.marked.clear();
    }

    /// The entry as plain text: the formatting, the image or the file list is left out of the paste.
    pub(crate) fn paste_as_text(&mut self, id: u64) {
        let text = {
            let h = self.shared.history.lock().unwrap_or_else(|e| e.into_inner());
            h.get(id)
                .filter(|i| !matches!(i.kind, Kind::Image | Kind::Files))
                .map(|i| i.text.clone())
        };
        match text {
            Some(t) => self.request_paste_text(t),
            None => self.request_paste(id),
        }
    }

    /// The Ctrl+clicked entries, one per line, in the order they were picked.
    pub(crate) fn paste_marked(&mut self) {
        let text = {
            let h = self.shared.history.lock().unwrap_or_else(|e| e.into_inner());
            self.popup
                .marked
                .iter()
                .filter_map(|id| h.get(*id))
                .filter(|i| !matches!(i.kind, Kind::Image | Kind::Files))
                .map(|i| i.text.as_str())
                .collect::<Vec<_>>()
                .join("\n")
        };
        self.popup.marked.clear();
        if !text.is_empty() {
            self.request_paste_text(text);
        }
    }

    pub(crate) fn toggle_marked(&mut self, id: u64) {
        match self.popup.marked.iter().position(|m| *m == id) {
            Some(p) => {
                self.popup.marked.remove(p);
            }
            None => self.popup.marked.push(id),
        }
    }

    pub(crate) fn apply_transform(&mut self, t: Transform, text: &str) {
        let lang = self.shared.lang();
        match t.apply(text) {
            Ok(out) => self.request_paste_text(out),
            Err(e) => {
                self.popup.toast = Some((format!("{}: {e}", S::TransformFailed.get(lang)), Instant::now()));
            }
        }
    }

    pub(crate) fn pin_selected(&mut self) {
        if let Some(id) = self.popup.selected {
            let mut h = self.shared.history.lock().unwrap_or_else(|e| e.into_inner());
            let pinned = h.get(id).is_some_and(|i| i.pinned);
            let _ = h.pin(id, !pinned);
        }
    }

    pub(crate) fn delete_selected(&mut self) {
        let Some(id) = self.popup.selected else { return };
        let next = self
            .selected_index()
            .and_then(|i| {
                self.popup
                    .results
                    .get(i + 1)
                    .or_else(|| i.checked_sub(1).and_then(|p| self.popup.results.get(p)))
            })
            .copied();
        let _ = self.shared.history.lock().unwrap_or_else(|e| e.into_inner()).delete(id);
        self.popup.selected = next;
    }

    fn keys(&mut self, ctx: &egui::Context) {
        let none = Modifiers::NONE;
        let (down, up, page_down, page_up, copy, plain, enter, esc, pin, del) = ctx.input_mut(|i| {
            (
                i.consume_key(none, Key::ArrowDown),
                i.consume_key(none, Key::ArrowUp),
                i.consume_key(none, Key::PageDown),
                i.consume_key(none, Key::PageUp),
                i.consume_key(Modifiers::COMMAND, Key::Enter),
                i.consume_key(Modifiers::SHIFT, Key::Enter),
                i.consume_key(none, Key::Enter),
                i.consume_key(none, Key::Escape),
                i.consume_key(Modifiers::COMMAND, Key::P),
                i.consume_key(Modifiers::COMMAND, Key::D),
            )
        });
        let digit = if self.popup.query.is_empty() {
            let keys = [
                Key::Num1,
                Key::Num2,
                Key::Num3,
                Key::Num4,
                Key::Num5,
                Key::Num6,
                Key::Num7,
                Key::Num8,
                Key::Num9,
            ];
            ctx.input_mut(|i| keys.iter().position(|k| i.consume_key(none, *k)))
        } else {
            None
        };
        if down {
            self.move_selection(1);
        }
        if up {
            self.move_selection(-1);
        }
        if page_down {
            self.move_selection(8);
        }
        if page_up {
            self.move_selection(-8);
        }
        if copy && let Some(id) = self.popup.selected {
            self.request_copy(id);
        }
        if (enter || plain)
            && let Some((sid, _)) = self.popup.keyword_snippet.clone()
        {
            self.paste_snippet(sid);
        } else if (enter || plain) && !self.popup.marked.is_empty() {
            self.paste_marked();
        } else if plain && let Some(id) = self.popup.selected {
            self.paste_as_text(id);
        } else if enter && let Some(id) = self.popup.selected {
            self.request_paste(id);
        }
        if let Some(n) = digit
            && let Some(&id) = self.popup.results.get(n)
        {
            self.request_paste(id);
        }
        if pin {
            self.pin_selected();
        }
        if del {
            self.delete_selected();
        }
        if esc {
            if !self.popup.query.is_empty() {
                self.popup.query.clear();
            } else if !self.popup.marked.is_empty() {
                self.popup.marked.clear();
            } else if self.popup.view == View::Settings {
                self.popup.view = View::History;
            } else if !self.shared.demo {
                platform::hide_popup();
            }
        }
    }

    pub fn popup_ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let now = now_unix();
        let seq = self.shared.show_seq.load(Ordering::SeqCst);
        if seq != self.popup.seen_seq {
            self.popup.seen_seq = seq;
            self.popup.reset_filters();
            self.popup.view = if self.shared.open_settings.swap(false, Ordering::SeqCst) {
                View::Settings
            } else {
                View::History
            };
            self.popup.shown_at = Instant::now();
            self.popup.was_focused = false;
            platform::capture_end();
            let _ = self.shared.history.lock().unwrap_or_else(|e| e.into_inner()).refresh();
        }
        // hide when the window loses the focus, like Win+V does
        let focused = ctx.input(|i| i.viewport().focused.unwrap_or(true));
        if focused {
            self.popup.was_focused = true;
        } else if self.popup.was_focused
            && self.popup.shown_at.elapsed() > Duration::from_millis(400)
            && !self.shared.demo
            && self.popup.view == View::History
            && self.popup.drag.is_none()
        {
            platform::hide_popup();
            self.popup.was_focused = false;
        }

        self.refresh_results(now);
        self.popup.keyword_snippet = {
            let q = self.popup.query.trim();
            (!q.is_empty())
                .then(|| {
                    let h = self.shared.history.lock().unwrap_or_else(|e| e.into_inner());
                    h.snippet_by_keyword(q).map(|s| (s.id, s.name.clone()))
                })
                .flatten()
        };
        if self.popup.view == View::History {
            self.keys(&ctx);
        } else if self.popup.view == View::Snippets
            && self.popup.capturing.is_none()
            && ctx.input(|i| i.key_pressed(Key::Escape))
        {
            platform::hide_popup();
        }

        let compact =
            self.popup.view == View::History && !self.shared.demo && self.shared.compact.load(Ordering::SeqCst);
        if compact {
            egui::Panel::top("header").show(ui, |ui| self.compact_header(ui));
            egui::Panel::bottom("footer").show(ui, |ui| self.compact_footer(ui));
            egui::CentralPanel::default().show(ui, |ui| self.compact_list(ui, now));
            return;
        }
        egui::Panel::top("header").show(ui, |ui| self.header(ui));
        egui::Panel::bottom("footer").show(ui, |ui| self.footer(ui));
        match self.popup.view {
            View::Settings => {
                egui::CentralPanel::default().show(ui, |ui| self.settings_ui(ui));
            }
            View::Snippets => self.snippets_ui(ui),
            View::History => {
                egui::Panel::left("list")
                    .resizable(false)
                    .exact_size(400.0)
                    .show(ui, |ui| self.list(ui, now));
                egui::CentralPanel::default().show(ui, |ui| self.preview(ui, now));
            }
        }
    }

    /// The title row doubles as the handle of the borderless window. It is registered before the buttons so that
    /// they sit on top of it and still get their clicks; the window follows the mouse wherever it goes, across
    /// monitors included.
    pub(crate) fn drag_strip(&mut self, ui: &mut Ui, height: f32) {
        if self.shared.demo {
            return;
        }
        let row = ui.available_rect_before_wrap();
        let area = egui::Rect::from_min_size(row.min, vec2(row.width(), height));
        let resp = ui.interact(area, ui.id().with("drag"), Sense::drag());
        if resp.is_pointer_button_down_on() {
            if self.popup.drag.is_none() {
                self.popup.drag = platform::drag_begin();
            }
        } else if self.popup.drag.take().is_some() && self.shared.compact.load(Ordering::SeqCst) {
            // the compact window was let go: remember where, for the "where I left it" setting
            if let Some(pos) = platform::window_pos() {
                let mut st = self.shared.settings.lock().unwrap_or_else(|e| e.into_inner());
                if st.compact_last != Some(pos) {
                    st.compact_last = Some(pos);
                    st.save();
                }
            }
        }
        if resp.dragged()
            && let Some(d) = self.popup.drag
        {
            platform::drag_to(&d);
            // the window moves under a still pointer, so no mouse event would ask for the next frame
            ui.ctx().request_repaint();
        }
    }

    fn header(&mut self, ui: &mut Ui) {
        let lang = self.shared.lang();
        let (paused, incognito) = (
            self.shared.settings.lock().unwrap_or_else(|e| e.into_inner()).paused,
            self.shared.incognito.load(Ordering::Relaxed),
        );
        self.drag_strip(ui, 28.0);
        ui.horizontal(|ui| {
            if let Some(icon) = &self.icon {
                ui.add(egui::Image::new(icon).fit_to_exact_size(vec2(22.0, 22.0)));
            }
            ui.label(RichText::new("Clipvault").strong().size(16.0));
            for (view, label) in [(View::History, S::TabHistory), (View::Snippets, S::TabSnippets)] {
                if self.popup.view != View::Settings
                    && ui.selectable_label(self.popup.view == view, label.get(lang)).clicked()
                {
                    self.popup.view = view;
                }
            }
            if self.shared.admin {
                theme::badge(ui, S::AdminBadge.get(lang), theme::BAD);
            }
            if self.shared.demo {
                theme::badge(ui, S::DemoBanner.get(lang), theme::WARN);
            } else if paused {
                theme::badge(ui, S::PausedBanner.get(lang), theme::WARN);
            } else if incognito {
                theme::badge(ui, S::IncognitoBanner.get(lang), theme::ACCENT);
            }
            let available = match &*self.update.lock().unwrap_or_else(|e| e.into_inner()) {
                crate::update::State::Available { version, .. } => Some(version.clone()),
                _ => None,
            };
            if let Some(v) = available
                && theme::primary_button(ui, &format!("{} {v}", S::UpdateInstall.get(lang))).clicked()
            {
                self.start_update_check(true);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                // closing only hides the window: the app keeps running in the tray, whose menu has "Exit"
                if ui.button("×").on_hover_text(S::BtnCloseWindow.get(lang)).clicked() && !self.shared.demo {
                    platform::hide_popup();
                }
                if ui.button("—").on_hover_text(S::BtnMinimize.get(lang)).clicked() && !self.shared.demo {
                    platform::hide_popup();
                }
                let label = if self.popup.view == View::Settings {
                    S::Back
                } else {
                    S::TraySettings
                };
                if ui.button(label.get(lang)).clicked() {
                    self.popup.view = if self.popup.view == View::Settings {
                        View::History
                    } else {
                        View::Settings
                    };
                }
                if ui.selectable_label(incognito, S::Incognito.get(lang)).clicked() {
                    self.shared.incognito.fetch_xor(true, Ordering::SeqCst);
                    platform::settings_changed(&self.shared.settings.lock().unwrap_or_else(|e| e.into_inner()));
                }
                let pause = if paused { S::Resume } else { S::PauseRecording };
                if ui.selectable_label(paused, pause.get(lang)).clicked() {
                    let mut st = self.shared.settings.lock().unwrap_or_else(|e| e.into_inner());
                    st.paused = !st.paused;
                    st.save();
                    drop(st);
                    platform::post(platform::WM_REHOTKEY);
                }
            });
        });
        if self.popup.view == View::History {
            self.filters(ui, lang);
        }
        ui.add_space(2.0);
    }

    pub(crate) fn search_box(&mut self, ui: &mut Ui, lang: Lang) {
        let search = ui.add(
            egui::TextEdit::singleline(&mut self.popup.query)
                .hint_text(S::SearchHint.get(lang))
                .desired_width(f32::INFINITY)
                .font(FontId::proportional(15.0))
                .margin(vec2(8.0, 6.0)),
        );
        if self.popup.focus_search {
            search.request_focus();
            self.popup.focus_search = false;
        }
        if let Some((_, name)) = &self.popup.keyword_snippet {
            ui.label(RichText::new(clipvault_core::i18n::fmt(S::SnipKeywordHint, lang, &[name])).color(theme::GOOD));
        }
    }

    fn filters(&mut self, ui: &mut Ui, lang: Lang) {
        self.search_box(ui, lang);
        ui.horizontal_wrapped(|ui| {
            if theme::chip(ui, S::All.get(lang), self.popup.kind.is_none(), theme::ACCENT).clicked() {
                self.popup.kind = None;
            }
            for k in Kind::ALL {
                let on = self.popup.kind == Some(k);
                if theme::chip(ui, self.shared.kind_label(k), on, theme::kind_color(k)).clicked() {
                    self.popup.kind = if on { None } else { Some(k) };
                }
            }
            ui.separator();
            let apps = self.shared.history.lock().unwrap_or_else(|e| e.into_inner()).apps();
            egui::ComboBox::from_id_salt("app")
                .selected_text(
                    self.popup
                        .app
                        .clone()
                        .unwrap_or_else(|| S::AllApps.get(lang).to_owned()),
                )
                .width(150.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.popup.app, None, S::AllApps.get(lang));
                    for a in apps {
                        ui.selectable_value(&mut self.popup.app, Some(a.clone()), a);
                    }
                });
            let date_label = |d: DateFilter| match d {
                DateFilter::Any => S::DateAny,
                DateFilter::Today => S::DateToday,
                DateFilter::Week => S::DateWeek,
                DateFilter::Month => S::DateMonth,
            };
            egui::ComboBox::from_id_salt("date")
                .selected_text(date_label(self.popup.date).get(lang))
                .width(120.0)
                .show_ui(ui, |ui| {
                    for d in [DateFilter::Any, DateFilter::Today, DateFilter::Week, DateFilter::Month] {
                        ui.selectable_value(&mut self.popup.date, d, date_label(d).get(lang));
                    }
                });
            ui.toggle_value(&mut self.popup.pinned_only, S::PinnedOnly.get(lang));
        });
    }

    fn footer(&mut self, ui: &mut Ui) {
        let lang = self.shared.lang();
        ui.horizontal(|ui| {
            let total = self.shared.history.lock().unwrap_or_else(|e| e.into_inner()).len();
            ui.label(
                RichText::new(format!(
                    "{} / {total} {}",
                    self.popup.results.len(),
                    S::EntriesCount.get(lang)
                ))
                .weak()
                .small(),
            );
            if let Some((t, at)) = &self.popup.toast {
                if at.elapsed() < Duration::from_secs(4) {
                    ui.label(RichText::new(t).color(theme::GOOD).small());
                    ui.ctx().request_repaint_after(Duration::from_millis(500));
                } else {
                    self.popup.toast = None;
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if self.popup.confirm_clear {
                    ui.label(S::ClearConfirm.get(lang));
                    if ui.button(S::Cancel.get(lang)).clicked() {
                        self.popup.confirm_clear = false;
                    }
                    for (label, keep) in [(S::ClearEverything, false), (S::ClearKeepPinned, true)] {
                        if ui.button(RichText::new(label.get(lang)).color(theme::BAD)).clicked() {
                            self.clear_everything(keep);
                        }
                    }
                } else {
                    if ui.button(S::ClearAll.get(lang)).clicked() {
                        self.popup.confirm_clear = true;
                    }
                    ui.label(RichText::new(S::HintsLine.get(lang)).weak().small());
                }
            });
        });
    }

    fn list(&mut self, ui: &mut Ui, now: u64) {
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
        let mut action: Option<(u64, RowAction)> = None;
        let mut clicked: Option<u64> = None;
        let total = self.popup.results.len();
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show_rows(ui, ROW_H, total, |ui, range| {
                if scroll && let Some(sel) = selected {
                    let top = sel as f32 * ROW_H;
                    ui.scroll_to_rect(
                        egui::Rect::from_min_size(
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
                    let is_sel = selected == Some(idx);
                    let resp = row(ui, item, idx, is_sel, now, lang, &self.shared);
                    if resp.clicked() {
                        clicked = Some(item.id);
                    }
                    if resp.double_clicked() {
                        action = Some((item.id, RowAction::Paste));
                    }
                    let id = item.id;
                    if let Some(a) = row_menu(&resp, item, lang) {
                        action = Some((id, a));
                    }
                }
            });
        if let Some(id) = clicked {
            self.popup.selected = Some(id);
        }
        if let Some((id, a)) = action {
            self.row_action(id, a);
        }
    }

    /// What a click or a menu choice on an entry does; the entry becomes the selected one.
    pub(crate) fn row_action(&mut self, id: u64, a: RowAction) {
        self.popup.selected = Some(id);
        match a {
            RowAction::Paste => self.request_paste(id),
            RowAction::PasteText => self.paste_as_text(id),
            RowAction::Copy => self.request_copy(id),
            RowAction::Mark => self.toggle_marked(id),
            RowAction::Pin => self.pin_selected(),
            RowAction::Delete => self.delete_selected(),
            RowAction::Transform(t) => {
                let text = self
                    .shared
                    .history
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .get(id)
                    .map(|i| i.text.clone());
                if let Some(text) = text {
                    self.apply_transform(t, &text);
                }
            }
        }
    }

    fn preview(&mut self, ui: &mut Ui, now: u64) {
        let lang = self.shared.lang();
        let Some(id) = self.popup.selected else {
            ui.add_space(60.0);
            ui.vertical_centered(|ui| ui.label(RichText::new(S::SelectEntry.get(lang)).weak()));
            return;
        };
        let (item, blob) = {
            let h = self.shared.history.lock().unwrap_or_else(|e| e.into_inner());
            let Some(item) = h.get(id).cloned() else { return };
            let blob = (item.kind == Kind::Image).then(|| h.blob(id).ok().flatten()).flatten();
            (item, blob)
        };
        ui.horizontal(|ui| {
            if theme::primary_button(ui, S::Paste.get(lang)).clicked() {
                self.request_paste(id);
            }
            if !matches!(item.kind, Kind::Image) {
                ui.menu_button(S::TransformMenu.get(lang), |ui| {
                    for t in Transform::ALL {
                        if ui.button(transform_label(t).get(lang)).clicked() {
                            self.apply_transform(t, &item.text);
                            ui.close();
                        }
                    }
                });
            }
            let pin = if item.pinned { S::Unpin } else { S::Pin };
            if ui.button(pin.get(lang)).clicked() {
                self.pin_selected();
            }
            if ui
                .button(RichText::new(S::Delete.get(lang)).color(theme::BAD))
                .clicked()
            {
                self.delete_selected();
            }
        });
        ui.add_space(4.0);
        let meta_height = 124.0;
        let body_h = (ui.available_height() - meta_height).max(80.0);
        theme::card().show(ui, |ui| {
            ui.set_min_size(vec2(ui.available_width(), body_h - 24.0));
            ui.set_max_height(body_h - 24.0);
            self.preview_body(ui, &item, blob.as_deref());
        });
        ui.add_space(4.0);
        meta_grid(ui, &item, now, lang, &self.shared);
    }

    fn preview_body(&mut self, ui: &mut Ui, item: &Item, blob: Option<&[u8]>) {
        match item.kind {
            Kind::Image => {
                let ctx = ui.ctx().clone();
                if self.popup.tex.as_ref().map(|(i, _)| *i) != Some(item.id) {
                    self.popup.tex = blob.and_then(clipfmt::png_decode).map(|img| {
                        let ci = egui::ColorImage::from_rgba_unmultiplied(
                            [img.width as usize, img.height as usize],
                            &img.pixels,
                        );
                        (item.id, ctx.load_texture("preview", ci, TextureOptions::LINEAR))
                    });
                }
                match &self.popup.tex {
                    Some((_, tex)) => {
                        egui::ScrollArea::both().show(ui, |ui| {
                            let size = tex.size_vec2();
                            let scale = (ui.available_width() / size.x).min(1.0);
                            ui.add(egui::Image::new(tex).fit_to_exact_size(size * scale));
                        });
                    }
                    None => {
                        ui.label(RichText::new("—").weak());
                    }
                }
            }
            Kind::Color => {
                if let Some([r, g, b, a]) = parse_color(&item.text) {
                    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 110.0), Sense::hover());
                    ui.painter()
                        .rect_filled(rect, 8.0, Color32::from_rgba_unmultiplied(r, g, b, a));
                    ui.add_space(6.0);
                    ui.label(RichText::new(&item.text).monospace().size(16.0));
                    ui.label(RichText::new(format!("#{r:02X}{g:02X}{b:02X}   rgb({r}, {g}, {b})")).monospace());
                    let (h, s, l) = rgb_to_hsl(r, g, b);
                    ui.label(RichText::new(format!("hsl({h}, {s}%, {l}%)")).monospace());
                } else {
                    ui.label(&item.text);
                }
            }
            _ if item.volatile && self.popup.revealed != Some(item.id) => {
                let lang = self.shared.lang();
                let mask = item
                    .secret
                    .map_or_else(|| "••••••••".to_owned(), |k| clipvault_core::guard::mask_text(k, lang));
                ui.label(RichText::new(mask).monospace().size(16.0));
                ui.add_space(6.0);
                if ui.button(S::Reveal.get(lang)).clicked() {
                    self.popup.revealed = Some(item.id);
                }
            }
            Kind::Files => {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for line in item.text.lines() {
                        ui.label(RichText::new(line).monospace());
                    }
                });
            }
            _ => {
                let text: String = item.text.chars().take(20_000).collect();
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    ui.add(egui::Label::new(RichText::new(text).monospace()).wrap());
                });
            }
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum RowAction {
    Paste,
    PasteText,
    Copy,
    /// Ctrl+click: add to / remove from the entries that are pasted together.
    Mark,
    Pin,
    Delete,
    Transform(Transform),
}

/// The right-click menu of an entry, shared by the full list and the compact one.
pub(crate) fn row_menu(resp: &egui::Response, item: &Item, lang: Lang) -> Option<RowAction> {
    let mut action = None;
    resp.context_menu(|ui| {
        if ui.button(S::Paste.get(lang)).clicked() {
            action = Some(RowAction::Paste);
            ui.close();
        }
        if ui.button(S::CopyOnly.get(lang)).clicked() {
            action = Some(RowAction::Copy);
            ui.close();
        }
        if !matches!(item.kind, Kind::Image | Kind::Files) {
            if item.kind == Kind::Rich && ui.button(S::PasteAsText.get(lang)).clicked() {
                action = Some(RowAction::PasteText);
                ui.close();
            }
            if !item.volatile {
                ui.menu_button(S::TransformMenu.get(lang), |ui| {
                    for t in Transform::ALL {
                        if ui.button(transform_label(t).get(lang)).clicked() {
                            action = Some(RowAction::Transform(t));
                            ui.close();
                        }
                    }
                });
            }
        }
        let pin = if item.pinned { S::Unpin } else { S::Pin };
        if ui.button(pin.get(lang)).clicked() {
            action = Some(RowAction::Pin);
            ui.close();
        }
        if ui.button(S::Delete.get(lang)).clicked() {
            action = Some(RowAction::Delete);
            ui.close();
        }
    });
    action
}

/// One line that stands for an entry in a list.
pub(crate) fn entry_label(item: &Item, lang: Lang) -> String {
    match item.kind {
        Kind::Image => {
            let (w, h) = item.img.unwrap_or((0, 0));
            format!("{} {w}×{h}", S::ImageLabel.get(lang))
        }
        Kind::Files => {
            let n = item.text.lines().count();
            let first = item
                .text
                .lines()
                .next()
                .unwrap_or("")
                .rsplit('\\')
                .next()
                .unwrap_or("")
                .to_owned();
            format!("{first}  ({} {})", S::FilesLabel.get(lang), n.saturating_sub(1))
        }
        _ if item.volatile => item
            .secret
            .map_or_else(|| "••••••••".to_owned(), |k| clipvault_core::guard::mask_text(k, lang)),
        _ => one_line(&item.text, 90),
    }
}

fn row(
    ui: &mut Ui,
    item: &Item,
    idx: usize,
    selected: bool,
    now: u64,
    lang: Lang,
    shared: &crate::shared::Shared,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
    let p = ui.painter();
    if selected {
        p.rect_filled(rect.shrink2(vec2(0.0, 1.0)), 6.0, theme::ROW_SELECTED);
    } else if resp.hovered() {
        p.rect_filled(rect.shrink2(vec2(0.0, 1.0)), 6.0, theme::ROW_HOVER);
    }
    let kind_col = theme::kind_color(item.kind);
    let x0 = rect.left() + 8.0;
    // number badge for the first nine entries
    if idx < 9 {
        p.text(
            egui::pos2(x0 + 6.0, rect.center().y),
            Align2::CENTER_CENTER,
            (idx + 1).to_string(),
            FontId::monospace(13.0),
            Color32::from_gray(150),
        );
    }
    let sw = egui::Rect::from_center_size(egui::pos2(x0 + 30.0, rect.center().y), vec2(14.0, 14.0));
    match (item.kind, parse_color(&item.text)) {
        (Kind::Color, Some([r, g, b, a])) => {
            p.rect_filled(sw, 4.0, Color32::from_rgba_unmultiplied(r, g, b, a));
        }
        _ => {
            p.rect_filled(sw, 4.0, kind_col.gamma_multiply(0.9));
        }
    }
    let text_x = x0 + 48.0;
    let label = entry_label(item, lang);
    let right = format!(
        "{}{}",
        if item.source_app.is_empty() {
            String::new()
        } else {
            format!("{} · ", item.source_app.trim_end_matches(".exe"))
        },
        ago(lang, now, item.last_used)
    );
    let right_galley = p.layout_no_wrap(right, FontId::proportional(11.0), Color32::from_gray(140));
    let right_w = right_galley.size().x.min(150.0);
    let clip = egui::Rect::from_min_max(
        egui::pos2(text_x, rect.top()),
        egui::pos2(rect.right() - right_w - 12.0, rect.bottom()),
    );
    let main_col = if item.masked || item.volatile {
        Color32::from_gray(160)
    } else {
        Color32::from_gray(225)
    };
    p.with_clip_rect(clip).text(
        egui::pos2(text_x, rect.top() + 12.0),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(14.0),
        main_col,
    );
    let mut sub = shared.kind_label(item.kind).to_owned();
    if item.count > 1 {
        sub.push_str(&format!(" · ×{}", item.count));
    }
    if item.volatile {
        sub.push_str(&format!(" · {}", S::InMemoryOnly.get(lang)));
    } else if item.masked {
        sub.push_str(&format!(" · {}", S::Masked.get(lang)));
    }
    p.with_clip_rect(clip).text(
        egui::pos2(text_x, rect.bottom() - 11.0),
        Align2::LEFT_CENTER,
        sub,
        FontId::proportional(11.0),
        kind_col,
    );
    let pos = egui::pos2(rect.right() - 8.0 - right_w, rect.top() + 12.0);
    p.with_clip_rect(egui::Rect::from_min_max(
        pos,
        egui::pos2(rect.right() - 6.0, rect.bottom()),
    ))
    .galley(pos, right_galley, Color32::from_gray(140));
    if item.pinned {
        p.text(
            egui::pos2(rect.right() - 14.0, rect.bottom() - 11.0),
            Align2::CENTER_CENTER,
            "📌",
            FontId::proportional(12.0),
            theme::WARN,
        );
    }
    resp
}

fn meta_grid(ui: &mut Ui, item: &Item, now: u64, lang: Lang, shared: &crate::shared::Shared) {
    egui::Grid::new("meta")
        .num_columns(2)
        .spacing([14.0, 3.0])
        .show(ui, |ui| {
            let mut line = |k: S, v: String| {
                ui.label(RichText::new(k.get(lang)).weak());
                ui.label(v);
                ui.end_row();
            };
            line(S::Type, shared.kind_label(item.kind).to_owned());
            let src = match (item.source_app.is_empty(), item.source_title.is_empty()) {
                (true, _) => "—".to_owned(),
                (false, true) => item.source_app.clone(),
                (false, false) => format!("{} — {}", item.source_app, one_line(&item.source_title, 50)),
            };
            line(S::Source, src);
            line(
                S::Copied,
                format!(
                    "{} ({} UTC)",
                    ago(lang, now, item.last_used),
                    clipvault_core::timefmt::format_utc(item.created)
                ),
            );
            line(S::Repeats, item.count.to_string());
            // the size of a hidden secret would reveal its length
            let size = if item.volatile {
                "—".to_owned()
            } else {
                human_size((item.text.len() as u64).max(item.blob_len))
            };
            line(S::Size, size);
        });
}

fn rgb_to_hsl(r: u8, g: u8, b: u8) -> (u32, u32, u32) {
    let (r, g, b) = (f64::from(r) / 255.0, f64::from(g) / 255.0, f64::from(b) / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let l = (max + min) / 2.0;
    let d = max - min;
    if d == 0.0 {
        return (0, 0, (l * 100.0).round() as u32);
    }
    let s = d / (1.0 - (2.0 * l - 1.0).abs());
    let h = if max == r {
        ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    } * 60.0;
    (
        h.round() as u32 % 360,
        (s * 100.0).round() as u32,
        (l * 100.0).round() as u32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hsl_conversion() {
        assert_eq!(rgb_to_hsl(255, 0, 0), (0, 100, 50));
        assert_eq!(rgb_to_hsl(0, 0, 255), (240, 100, 50));
        assert_eq!(rgb_to_hsl(128, 128, 128), (0, 0, 50));
    }

    #[test]
    fn relative_time() {
        assert_eq!(ago(Lang::En, 1000, 990), "just now");
        assert_eq!(ago(Lang::En, 1000, 1000 - 5 * 60), "5 min ago");
        assert_eq!(ago(Lang::Ru, 100_000, 100_000 - 3 * 3600), "3 ч назад");
        assert_eq!(human_size(2048), "2.0 KB");
        assert_eq!(one_line("a \n\n  b\tc", 50), "a b c");
    }
}
