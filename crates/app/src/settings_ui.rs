//! The settings view inside the popup window.

use std::time::{Duration, Instant};

use clipvault_core::config::{CompactPosition, Lang};
use clipvault_core::i18n::S;
use eframe::egui::{self, RichText, Ui};

use crate::{App, diag, platform, update, winint};

/// Writes the settings and re-registers the global hotkeys, but only when something changed.
fn drop_and_apply(st: std::sync::MutexGuard<'_, clipvault_core::config::Settings>, changed: bool) {
    if changed {
        st.save();
    }
    drop(st);
    if changed {
        platform::post(platform::WM_REHOTKEY);
    }
}

/// A button that shows a hotkey and records a new one when it is clicked: the next combination pressed on the
/// physical keyboard (modifiers + a key) is taken, Esc cancels and, when `clearable`, Backspace empties the field.
/// Returns true when the value changed.
pub(crate) fn hotkey_field(
    ui: &mut Ui,
    capturing: &mut Option<egui::Id>,
    salt: impl std::hash::Hash + std::fmt::Debug,
    value: &mut String,
    clearable: bool,
    lang: Lang,
) -> bool {
    let me = ui.make_persistent_id(salt);
    let mut changed = false;
    if *capturing == Some(me) {
        let focused = ui.ctx().input(|i| i.viewport().focused.unwrap_or(true));
        let seen = if focused {
            platform::poll_capture()
        } else {
            platform::Capture::Cancel
        };
        let done = match seen {
            platform::Capture::Got(spec) => {
                *value = spec;
                changed = true;
                true
            }
            platform::Capture::Clear if clearable => {
                value.clear();
                changed = true;
                true
            }
            platform::Capture::Cancel => true,
            _ => false,
        };
        if done {
            *capturing = None;
            platform::capture_end();
        } else {
            let hint = if clearable { S::PressKeysClear } else { S::PressKeys };
            ui.label(RichText::new(hint.get(lang)).color(crate::theme::ACCENT));
            if ui.button(S::Cancel.get(lang)).clicked() {
                *capturing = None;
                platform::capture_end();
            }
            // nothing else would ask for a frame while the keys are being pressed
            ui.ctx().request_repaint_after(Duration::from_millis(30));
        }
    }
    if *capturing != Some(me) {
        let shown = if value.trim().is_empty() {
            S::HotkeyNotSet.get(lang).to_owned()
        } else {
            value.clone()
        };
        if ui
            .button(RichText::new(shown).monospace().strong())
            .on_hover_text(S::RecordHotkey.get(lang))
            .clicked()
        {
            *capturing = Some(me);
            platform::capture_begin();
        }
        if clearable && !value.trim().is_empty() && ui.small_button("×").on_hover_text(S::Remove.get(lang)).clicked() {
            value.clear();
            changed = true;
        }
    }
    changed
}

impl App {
    pub fn settings_ui(&mut self, ui: &mut Ui) {
        let lang = self.shared.lang();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.heading(S::SettingsTitle.get(lang));
            ui.add_space(6.0);
            self.sec_general(ui, lang);
            ui.add_space(8.0);
            self.sec_hotkeys(ui, lang);
            ui.add_space(8.0);
            self.sec_secrets(ui, lang);
            ui.add_space(8.0);
            self.sec_limits(ui, lang);
            ui.add_space(8.0);
            if !self.shared.demo {
                self.sec_admin(ui, lang);
                ui.add_space(8.0);
            }
            self.sec_updates(ui, lang);
            ui.add_space(8.0);
            self.sec_diag(ui, lang);
            if !self.shared.demo {
                ui.add_space(8.0);
                self.sec_install(ui, lang);
            }
        });
    }

    fn sec_general(&mut self, ui: &mut Ui, lang: Lang) {
        crate::theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(S::SecGeneral.get(lang)).strong());
            let mut st = self.shared.settings.lock().unwrap_or_else(|e| e.into_inner());
            let mut reregister = false;
            ui.horizontal(|ui| {
                ui.label(S::Language.get(lang));
                let before = st.lang;
                egui::ComboBox::from_id_salt("lang")
                    .selected_text(if st.lang == Lang::Ru {
                        "Русский"
                    } else {
                        "English"
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut st.lang, Lang::Ru, "Русский");
                        ui.selectable_value(&mut st.lang, Lang::En, "English");
                    });
                if st.lang != before {
                    st.save();
                }
            });
            ui.horizontal(|ui| {
                ui.label(S::PopupHotkey.get(lang));
                if hotkey_field(
                    ui,
                    &mut self.popup.capturing,
                    "popup_hotkey",
                    &mut st.hotkey,
                    false,
                    lang,
                ) {
                    st.save();
                    reregister = true;
                }
            });
            if reregister {
                platform::post(platform::WM_REHOTKEY);
            }
            ui.label(RichText::new(S::HotkeyWinNote.get(lang)).weak().small());
            if let Some(e) = self
                .shared
                .hotkey_error
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .as_ref()
            {
                ui.label(RichText::new(e).color(crate::theme::BAD));
            }
            let win_v = st.replace_win_v;
            if !win_v {
                ui.label(RichText::new(S::HotkeyNote.get(lang)).weak().small());
            }
            let mut at_cursor = st.popup_at_cursor;
            let mut compact = st.compact_popup;
            let mut cpos = st.compact_position;
            drop(st);
            if ui.checkbox(&mut compact, S::CompactPopup.get(lang)).changed() {
                let mut st = self.shared.settings.lock().unwrap_or_else(|e| e.into_inner());
                st.compact_popup = compact;
                st.save();
            }
            ui.horizontal(|ui| {
                ui.label(S::CompactPosLabel.get(lang));
                let name = |p: CompactPosition| match p {
                    CompactPosition::Cursor => S::PosCursor,
                    CompactPosition::Center => S::PosCenter,
                    CompactPosition::Corner => S::PosCorner,
                    CompactPosition::Last => S::PosLast,
                    CompactPosition::Caret => S::PosCaret,
                };
                let before = cpos;
                egui::ComboBox::from_id_salt("cpos")
                    .selected_text(name(cpos).get(lang))
                    .show_ui(ui, |ui| {
                        for p in [
                            CompactPosition::Cursor,
                            CompactPosition::Center,
                            CompactPosition::Corner,
                            CompactPosition::Caret,
                            CompactPosition::Last,
                        ] {
                            ui.selectable_value(&mut cpos, p, name(p).get(lang));
                        }
                    });
                if cpos != before {
                    let mut st = self.shared.settings.lock().unwrap_or_else(|e| e.into_inner());
                    st.compact_position = cpos;
                    st.save();
                }
            });
            if compact && matches!(cpos, CompactPosition::Cursor | CompactPosition::Caret) {
                self.offset_editor(ui, lang, cpos == CompactPosition::Caret);
            }
            self.compact_options(ui, lang, cpos == CompactPosition::Corner);
            let mut close = self
                .shared
                .settings
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .close_after_paste;
            if ui.checkbox(&mut close, S::CloseAfterPaste.get(lang)).changed() {
                let mut st = self.shared.settings.lock().unwrap_or_else(|e| e.into_inner());
                st.close_after_paste = close;
                st.save();
            }
            if !close {
                ui.label(RichText::new(S::CloseAfterPasteNote.get(lang)).weak().small());
            }
            if !self.shared.demo {
                let mut choice = win_v;
                ui.horizontal(|ui| {
                    ui.label(S::QuickPasteMode.get(lang));
                    ui.radio_value(&mut choice, false, S::WinVWindows.get(lang));
                    ui.radio_value(&mut choice, true, S::WinVClipvault.get(lang));
                });
                if choice != win_v && winint::set_win_v_released(choice).is_ok() {
                    let mut st = self.shared.settings.lock().unwrap_or_else(|e| e.into_inner());
                    st.replace_win_v = choice;
                    st.save();
                    drop(st);
                    self.popup.win_v_hint = true;
                    platform::post(platform::WM_REHOTKEY);
                }
                if choice {
                    ui.label(RichText::new(S::WinVNote.get(lang)).weak().small());
                }
                if self.popup.win_v_hint {
                    ui.label(RichText::new(S::WinVRestart.get(lang)).color(crate::theme::ACCENT));
                    if ui.button(S::WinVRestartBtn.get(lang)).clicked() {
                        winint::restart_explorer();
                        self.popup.win_v_hint = false;
                        // the new Explorer has to be up before the hotkey can be taken
                        std::thread::spawn(|| {
                            std::thread::sleep(std::time::Duration::from_secs(4));
                            platform::post(platform::WM_REHOTKEY);
                        });
                    }
                }
            }
            if ui.checkbox(&mut at_cursor, S::PopupAtCursor.get(lang)).changed() {
                let mut st = self.shared.settings.lock().unwrap_or_else(|e| e.into_inner());
                st.popup_at_cursor = at_cursor;
                st.save();
            }
            if !self.shared.demo {
                let mut auto = winint::autostart_enabled();
                if ui.checkbox(&mut auto, S::Autostart.get(lang)).changed()
                    && let Ok(exe) = std::env::current_exe()
                {
                    let _ = winint::set_autostart(auto, &exe);
                }
            }
        });
    }

    /// Size of the compact window, the corner gap and the behaviour around closing and pasting.
    fn compact_options(&mut self, ui: &mut Ui, lang: Lang, corner: bool) {
        let mut st = self.shared.settings.lock().unwrap_or_else(|e| e.into_inner());
        let before = st.clone();
        ui.horizontal(|ui| {
            ui.label(S::CompactSize.get(lang));
            ui.add(
                egui::DragValue::new(&mut st.compact_size.0)
                    .range(260..=1200)
                    .suffix(" px"),
            );
            ui.label("×");
            ui.add(
                egui::DragValue::new(&mut st.compact_size.1)
                    .range(300..=1600)
                    .suffix(" px"),
            );
            if ui.small_button(S::OffsetReset.get(lang)).clicked() {
                st.compact_size = clipvault_core::config::DEFAULT_COMPACT_SIZE;
            }
        });
        if corner {
            ui.horizontal(|ui| {
                ui.label(S::CompactMargin.get(lang));
                ui.add(
                    egui::DragValue::new(&mut st.compact_margin)
                        .range(0..=200)
                        .suffix(" px"),
                );
            });
        }
        ui.checkbox(&mut st.restore_focus_on_close, S::RestoreFocus.get(lang));
        ui.checkbox(&mut st.close_on_blur, S::CloseOnBlur.get(lang));
        ui.checkbox(&mut st.paste_shift_insert, S::PasteShiftInsert.get(lang));
        if *st != before {
            st.save();
        }
    }

    /// A diagram of the screen around the mouse cursor (or the text caret): drag the little window to where it
    /// should open, or type the numbers.
    fn offset_editor(&mut self, ui: &mut Ui, lang: Lang, caret: bool) {
        use eframe::egui::{Color32, CursorIcon, Rect, Sense, Stroke, StrokeKind, pos2, vec2};
        // diagram scale: one logical point of the real window is K points here
        const K: f32 = 0.2;
        const REACH: i32 = 2500;
        let size = self
            .shared
            .settings
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .compact_size;
        let (win_w, win_h) = (size.0 as f32 * K, size.1 as f32 * K);
        let f = K / 0.3;
        ui.label(
            RichText::new(if caret { S::CaretHint } else { S::OffsetHint }.get(lang))
                .weak()
                .small(),
        );
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width().clamp(300.0, 560.0), 300.0), Sense::hover());
        let center = rect.center();
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 8.0, Color32::from_rgb(18, 19, 27));
        p.rect_stroke(rect, 8.0, Stroke::new(1.0, Color32::from_gray(60)), StrokeKind::Inside);
        let faint = Stroke::new(1.0, Color32::from_gray(45));
        p.line_segment([pos2(rect.left(), center.y), pos2(rect.right(), center.y)], faint);
        p.line_segment([pos2(center.x, rect.top()), pos2(center.x, rect.bottom())], faint);

        let mut off = {
            let st = self.shared.settings.lock().unwrap_or_else(|e| e.into_inner());
            if caret { st.caret_offset } else { st.compact_offset }
        };
        let set = |off: (i32, i32), save: bool| {
            let mut st = self.shared.settings.lock().unwrap_or_else(|e| e.into_inner());
            if caret {
                st.caret_offset = off;
            } else {
                st.compact_offset = off;
            }
            if save {
                st.save();
            }
        };
        let mut menu = Rect::from_min_size(center + vec2(off.0 as f32 * K, off.1 as f32 * K), vec2(win_w, win_h));
        // keep the window on the diagram, whatever the saved offset is
        let keep = |m: Rect| {
            Rect::from_min_size(
                pos2(
                    m.min
                        .x
                        .clamp(rect.left() + 2.0, (rect.right() - win_w - 2.0).max(rect.left() + 2.0)),
                    m.min
                        .y
                        .clamp(rect.top() + 2.0, (rect.bottom() - win_h - 2.0).max(rect.top() + 2.0)),
                ),
                vec2(win_w, win_h),
            )
        };
        menu = keep(menu);
        let resp = ui
            .interact(menu, ui.id().with(("offset_menu", caret)), Sense::drag())
            .on_hover_cursor(CursorIcon::Grab);
        if resp.dragged() {
            menu = keep(menu.translate(resp.drag_delta()));
            off = (
                ((menu.min.x - center.x) / K).round() as i32,
                ((menu.min.y - center.y) / K).round() as i32,
            );
            set(off, false);
        }
        if resp.drag_stopped() {
            set(off, true);
        }
        let hot = resp.hovered() || resp.dragged();
        p.rect_filled(
            menu,
            6.0,
            crate::theme::ACCENT.gamma_multiply(if hot { 0.55 } else { 0.4 }),
        );
        p.rect_stroke(menu, 6.0, Stroke::new(1.5, crate::theme::ACCENT), StrokeKind::Inside);
        // a few rows, so that it reads as the quick-paste list
        p.rect_filled(
            Rect::from_min_size(menu.min + vec2(5.0, 5.0), vec2(win_w - 10.0, 5.0)),
            2.0,
            Color32::from_white_alpha(70),
        );
        for i in 0..6 {
            p.rect_filled(
                Rect::from_min_size(
                    menu.min + vec2(5.0, (26.0 + i as f32 * 20.0) * f),
                    vec2(win_w - 10.0, 14.0 * f),
                ),
                2.0,
                Color32::from_white_alpha(28),
            );
        }
        // the anchor, on top of everything: the mouse cursor, or the bar that blinks where the text goes
        if caret {
            p.rect_filled(Rect::from_min_size(center, vec2(2.0, 16.0)), 0.0, Color32::WHITE);
            p.rect_stroke(
                Rect::from_min_size(center - vec2(1.0, 1.0), vec2(4.0, 18.0)),
                1.0,
                Stroke::new(1.0, Color32::BLACK),
                StrokeKind::Outside,
            );
        } else {
            let arrow = vec![
                center,
                center + vec2(0.0, 17.0),
                center + vec2(4.5, 13.0),
                center + vec2(8.0, 20.0),
                center + vec2(11.0, 18.5),
                center + vec2(7.5, 11.5),
                center + vec2(13.0, 11.0),
            ];
            p.add(egui::Shape::convex_polygon(
                arrow,
                Color32::WHITE,
                Stroke::new(1.0, Color32::BLACK),
            ));
        }
        p.text(
            center + vec2(18.0, 4.0),
            egui::Align2::LEFT_TOP,
            if caret { S::CaretLabel } else { S::CursorLabel }.get(lang),
            egui::FontId::proportional(11.0),
            Color32::from_gray(150),
        );
        ui.horizontal(|ui| {
            ui.label(S::OffsetX.get(lang));
            let mut changed = ui
                .add(egui::DragValue::new(&mut off.0).range(-REACH..=REACH).suffix(" px"))
                .changed();
            ui.label(S::OffsetY.get(lang));
            changed |= ui
                .add(egui::DragValue::new(&mut off.1).range(-REACH..=REACH).suffix(" px"))
                .changed();
            if changed {
                set(off, true);
            }
            if ui.button(S::OffsetReset.get(lang)).clicked() {
                set(
                    if caret {
                        clipvault_core::config::DEFAULT_CARET_OFFSET
                    } else {
                        clipvault_core::config::DEFAULT_COMPACT_OFFSET
                    },
                    true,
                );
            }
        });
    }

    fn sec_hotkeys(&mut self, ui: &mut Ui, lang: Lang) {
        use clipvault_core::config::TransformHotkey;
        use clipvault_core::transform::Transform;
        let card = crate::theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(S::PlainHotkey.get(lang)).strong());
            let mut st = self.shared.settings.lock().unwrap_or_else(|e| e.into_inner());
            let mut reregister = false;
            ui.horizontal(|ui| {
                if hotkey_field(
                    ui,
                    &mut self.popup.capturing,
                    "plain_hotkey",
                    &mut st.plain_paste_hotkey,
                    true,
                    lang,
                ) {
                    reregister = true;
                }
            });
            ui.label(RichText::new(S::PlainNote.get(lang)).weak().small());
            ui.add_space(6.0);
            ui.label(RichText::new(S::TransformHotkeys.get(lang)).strong());
            let mut remove: Option<usize> = None;
            for (i, th) in st.transform_hotkeys.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    if hotkey_field(ui, &mut self.popup.capturing, ("th", i), &mut th.hotkey, true, lang) {
                        reregister = true;
                    }
                    let current = Transform::parse(&th.transform);
                    egui::ComboBox::from_id_salt(("tt", i))
                        .selected_text(current.map_or("?", |t| crate::ui::transform_label(t).get(lang)))
                        .width(240.0)
                        .show_ui(ui, |ui| {
                            for t in Transform::ALL {
                                if ui
                                    .selectable_label(current == Some(t), crate::ui::transform_label(t).get(lang))
                                    .clicked()
                                {
                                    th.transform = t.name().to_owned();
                                    reregister = true;
                                }
                            }
                        });
                    if ui.button(S::Remove.get(lang)).clicked() {
                        remove = Some(i);
                    }
                });
            }
            if let Some(i) = remove {
                st.transform_hotkeys.remove(i);
                reregister = true;
            }
            if ui.button(S::AddRow.get(lang)).clicked() {
                st.transform_hotkeys.push(TransformHotkey {
                    hotkey: String::new(),
                    transform: Transform::Trim.name().to_owned(),
                });
                reregister = true;
            }
            drop_and_apply(st, reregister);
        });
        if std::mem::take(&mut self.popup.scroll_to_hotkeys) {
            ui.scroll_to_rect(card.response.rect, Some(egui::Align::TOP));
        }
    }

    fn sec_secrets(&mut self, ui: &mut Ui, lang: Lang) {
        use clipvault_core::guard::SecretPolicy;
        use clipvault_core::secrets::{Detector, Sensitivity};
        if !self.popup.texts_loaded {
            let st = self.shared.settings.lock().unwrap_or_else(|e| e.into_inner());
            self.popup.custom_text = st.custom_patterns.join(
                "
",
            );
            self.popup.blacklist_text = st.blacklist.join(
                "
",
            );
            self.popup.texts_loaded = true;
        }
        let card = crate::theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(S::SecretsSection.get(lang)).strong());
            let mut changed = false;
            let mut st = self.shared.settings.lock().unwrap_or_else(|e| e.into_inner());
            changed |= ui
                .checkbox(&mut st.secrets_enabled, S::SecretsEnabled.get(lang))
                .changed();
            ui.add_enabled_ui(st.secrets_enabled, |ui| {
                ui.label(S::PolicyLabel.get(lang));
                for (p, label) in [
                    (SecretPolicy::DontStore, S::PolicyDont),
                    (SecretPolicy::MemoryOnly, S::PolicyMemory),
                    (SecretPolicy::Mask, S::PolicyMask),
                ] {
                    changed |= ui.radio_value(&mut st.secret_policy, p, label.get(lang)).changed();
                }
                if st.secret_policy == SecretPolicy::MemoryOnly {
                    ui.horizontal(|ui| {
                        ui.label(S::ClearAfter.get(lang));
                        changed |= ui
                            .add(egui::DragValue::new(&mut st.secret_clear_secs).range(1..=3600))
                            .changed();
                    });
                }
                ui.horizontal(|ui| {
                    ui.label(S::SensitivityLabel.get(lang));
                    let name = |s: Sensitivity| match s {
                        Sensitivity::Off => S::SensOff,
                        Sensitivity::Low => S::SensLow,
                        Sensitivity::Medium => S::SensMedium,
                        Sensitivity::High => S::SensHigh,
                    };
                    egui::ComboBox::from_id_salt("sens")
                        .selected_text(name(st.password_sensitivity).get(lang))
                        .show_ui(ui, |ui| {
                            for s in [
                                Sensitivity::Off,
                                Sensitivity::Low,
                                Sensitivity::Medium,
                                Sensitivity::High,
                            ] {
                                changed |= ui
                                    .selectable_value(&mut st.password_sensitivity, s, name(s).get(lang))
                                    .changed();
                            }
                        });
                });
                ui.label(S::CustomPatterns.get(lang));
                if ui
                    .add(
                        egui::TextEdit::multiline(&mut self.popup.custom_text)
                            .desired_rows(2)
                            .desired_width(f32::INFINITY),
                    )
                    .changed()
                {
                    st.custom_patterns = self.popup.custom_text.lines().map(str::to_owned).collect();
                    changed = true;
                }
                let (_, bad) = Detector::new(Sensitivity::Off, &st.custom_patterns);
                if !bad.is_empty() {
                    let nums: Vec<String> = bad.iter().map(|i| (i + 1).to_string()).collect();
                    ui.label(
                        RichText::new(format!("{} {}", S::BadPatterns.get(lang), nums.join(", ")))
                            .color(crate::theme::BAD),
                    );
                }
            });
            changed |= ui
                .checkbox(&mut st.respect_app_flags, S::RespectFlags.get(lang))
                .changed();
            ui.label(S::BlacklistLabel.get(lang));
            if ui
                .add(
                    egui::TextEdit::multiline(&mut self.popup.blacklist_text)
                        .desired_rows(3)
                        .desired_width(f32::INFINITY),
                )
                .changed()
            {
                st.blacklist = self
                    .popup
                    .blacklist_text
                    .lines()
                    .map(|l| l.trim().to_owned())
                    .filter(|l| !l.is_empty())
                    .collect();
                changed = true;
            }
            ui.label(RichText::new(S::SecretsNote.get(lang)).weak().small());
            if changed {
                st.save();
            }
        });
        if std::mem::take(&mut self.popup.scroll_to_secrets) {
            ui.scroll_to_rect(card.response.rect, Some(egui::Align::TOP));
        }
    }

    fn sec_limits(&mut self, ui: &mut Ui, lang: Lang) {
        crate::theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(S::SecLimits.get(lang)).strong());
            let mut st = self.shared.settings.lock().unwrap_or_else(|e| e.into_inner());
            let before = (st.max_item_kb, st.max_age_days, st.max_items);
            egui::Grid::new("limits").num_columns(2).show(ui, |ui| {
                ui.label(S::MaxItemKb.get(lang));
                ui.add(egui::DragValue::new(&mut st.max_item_kb).range(1..=262_144));
                ui.end_row();
                ui.label(S::MaxAgeDays.get(lang));
                ui.add(egui::DragValue::new(&mut st.max_age_days).range(0..=36_500));
                ui.end_row();
                ui.label(S::MaxItems.get(lang));
                ui.add(egui::DragValue::new(&mut st.max_items).range(0..=1_000_000));
                ui.end_row();
            });
            ui.label(RichText::new(S::LimitsNote.get(lang)).weak().small());
            if (st.max_item_kb, st.max_age_days, st.max_items) != before {
                st.save();
                let limits = st.limits();
                drop(st);
                let _ = self
                    .shared
                    .history
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .enforce(&limits, clipvault_core::now_unix());
            }
        });
    }

    fn sec_admin(&mut self, ui: &mut Ui, lang: Lang) {
        crate::theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(S::AdminBadge.get(lang)).strong());
            ui.label(RichText::new(S::AdminNote.get(lang)).weak().small());
            ui.horizontal(|ui| {
                ui.label(S::AdminHotkey.get(lang));
                {
                    let mut st = self.shared.settings.lock().unwrap_or_else(|e| e.into_inner());
                    if hotkey_field(
                        ui,
                        &mut self.popup.capturing,
                        "admin_hotkey",
                        &mut st.admin_hotkey,
                        false,
                        lang,
                    ) {
                        st.save();
                    }
                }
                if !self.shared.admin
                    && !crate::winclip::self_elevated()
                    && ui.button(S::AdminOpen.get(lang)).clicked()
                    && let Err(e) = winint::open_as_admin()
                {
                    self.popup.toast = Some((e, Instant::now()));
                }
            });
        });
    }

    fn sec_updates(&mut self, ui: &mut Ui, lang: Lang) {
        crate::theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(S::SecUpdates.get(lang)).strong());
            {
                let mut st = self.shared.settings.lock().unwrap_or_else(|e| e.into_inner());
                if ui.checkbox(&mut st.auto_update, S::AutoUpdate.get(lang)).changed() {
                    st.save();
                }
            }
            ui.horizontal(|ui| {
                ui.label(format!("{} {}", S::Version.get(lang), env!("CARGO_PKG_VERSION")));
                let state = self.update.lock().unwrap_or_else(|e| e.into_inner()).clone();
                match &state {
                    update::State::Checking => {
                        ui.spinner();
                        ui.label(S::Checking.get(lang));
                    }
                    update::State::UpToDate => {
                        ui.label(RichText::new(S::UpToDate.get(lang)).color(crate::theme::GOOD));
                    }
                    update::State::Available { version, url } => {
                        ui.label(format!("{} {version}", S::UpdateAvailable.get(lang)));
                        ui.hyperlink_to(url, url);
                    }
                    update::State::Downloading { done, total, .. } => {
                        let frac = if *total > 0 { *done as f32 / *total as f32 } else { 0.0 };
                        ui.add(
                            egui::ProgressBar::new(frac)
                                .desired_width(160.0)
                                .text(S::UpdateDownloading.get(lang)),
                        );
                    }
                    update::State::Ready { .. } => {
                        ui.label(RichText::new(S::UpdateReady.get(lang)).color(crate::theme::GOOD));
                    }
                    update::State::Failed(e) => {
                        ui.label(RichText::new(format!("{}: {e}", S::UpdateFailed.get(lang))).color(crate::theme::BAD));
                    }
                    update::State::Idle => {}
                }
                let busy = matches!(state, update::State::Checking | update::State::Downloading { .. });
                if !self.shared.demo
                    && ui
                        .add_enabled(!busy, egui::Button::new(S::CheckNow.get(lang)))
                        .clicked()
                {
                    self.start_update_check(true);
                }
            });
        });
    }

    fn sec_diag(&mut self, ui: &mut Ui, lang: Lang) {
        crate::theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(S::SecDiagnostics.get(lang)).strong());
            ui.horizontal(|ui| {
                if ui.button(S::OpenLogs.get(lang)).clicked()
                    && let Some(d) = diag::logs_dir()
                {
                    let _ = std::fs::create_dir_all(&d);
                    let _ = std::process::Command::new("explorer.exe").arg(d).spawn();
                }
                if ui.button(S::CopyDiag.get(lang)).clicked() {
                    let counts = self.shared.history.lock().unwrap_or_else(|e| e.into_inner()).counts();
                    ui.ctx().copy_text(diag::report(&counts));
                    self.popup.toast = Some((S::DiagCopied.get(lang).to_owned(), Instant::now()));
                }
            });
            ui.label(RichText::new(S::DiagNote.get(lang)).weak().small());
        });
    }

    fn sec_install(&mut self, ui: &mut Ui, lang: Lang) {
        crate::theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(S::SecSystem.get(lang)).strong());
            if winint::running_installed() {
                ui.label(RichText::new(S::Installed.get(lang)).color(crate::theme::GOOD));
            } else if ui.button(S::InstallHere.get(lang)).clicked() {
                let opts = winint::InstallOptions {
                    desktop_shortcut: false,
                    autostart: true,
                };
                match winint::install(opts, false) {
                    Ok(exe) => {
                        // the new copy asks this one to exit and takes over
                        let _ = std::process::Command::new(exe).arg("--replace").spawn();
                    }
                    Err(e) => self.popup.toast = Some((e, Instant::now())),
                }
            }
        });
    }
}
